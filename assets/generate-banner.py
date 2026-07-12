#!/usr/bin/env python3
"""Regenerates assets/banner.svg -- the animated README banner.

Every visual choice here traces back to something real rather than invented:
- The spiral mark is website/components/sections/HeroMark.tsx's own path data, rotation origin
  (50,50), and rotation timing (website/app/globals.css: `slow-spin 36s linear infinite`).
- The palette (BG, ACCENT, ACCENT_STRONG, FG, FG_MUTED) is website/app/globals.css's own
  `.dark` block, not invented.
- NEBULA_VIOLET is the exact real RGB color PRODUCTION_BOOT_PROMPT.md's M7 stage-2 display probe
  proved on a real screen (crates/hyperion-init/src/linux/display_probe.rs).
- The wordmark and tagline positions are not computed by this script -- they were tuned by
  rendering the SVG in a real headless browser and reading back exact bounding boxes
  (getBoundingClientRect) until the whole composition (spiral + wordmark + tagline) sat centered
  in the canvas and the tagline's rendered width matched the wordmark's. LOCKUP_X/TITLE_Y/TAGLINE_Y
  and SPIRAL_TRANSLATE below are those measured results, recorded as constants rather than
  re-derived here, since that needs a real browser, not just this script.

Usage: python3 assets/generate-banner.py   (writes assets/banner.svg, relative to repo root)
"""

import random
from pathlib import Path

CANVAS_W = 1200
CANVAS_H = 260

# website/app/globals.css, .dark block -- the site's own real palette, not invented.
BG = "#0b0a08"
ACCENT = "#d9a54a"
ACCENT_STRONG = "#e6bb6e"
FG = "#f5f3ee"
FG_MUTED = "#a6a196"

# The same real color PRODUCTION_BOOT_PROMPT.md's M7 stage-2 display probe proved on a real
# screen, reused here as a nebula tint rather than an arbitrary purple.
NEBULA_VIOLET = "#4a2c6b"
NEBULA_BLUE = "#1a2a52"

STAR_SEED = 42
STAR_COUNT = 55

# Measured (see module docstring): centers the spiral+wordmark+tagline lockup as a whole in the
# 1200x260 canvas, and left-aligns the tagline under the wordmark.
SPIRAL_TRANSLATE = (312.24, 53.15)
SPIRAL_SCALE = 0.94
LOCKUP_X = 429.74
TITLE_Y = 127.5
TAGLINE_Y = 192.5

# website/components/sections/HeroMark.tsx's own spiral path, unmodified -- viewBox "5 5 90 90",
# rotates around its own native (50,50) center via that component's `origin-[50px_50px]`.
HERO_MARK_PATH = (
    "M 55.00 50.00 L 55.04 50.16 L 55.08 50.33 L 55.12 50.50 L 55.16 50.67 L 55.21 50.85 "
    "L 55.25 51.04 L 55.30 51.23 L 55.34 51.42 L 55.39 51.62 L 55.43 51.83 L 55.46 52.04 "
    "L 55.49 52.25 L 55.52 52.48 L 55.54 52.71 L 55.55 52.94 L 55.55 53.18 L 55.55 53.42 "
    "L 55.54 53.66 L 55.51 53.91 L 55.47 54.16 L 55.43 54.40 L 55.37 54.65 L 55.29 54.90 "
    "L 55.21 55.14 L 55.11 55.38 L 54.99 55.61 L 54.86 55.84 L 54.72 56.06 L 54.56 56.26 "
    "L 54.39 56.46 L 54.21 56.65 L 54.02 56.83 L 53.82 57.01 L 53.62 57.18 L 53.41 57.34 "
    "L 53.19 57.50 L 52.97 57.66 L 52.74 57.81 L 52.50 57.95 L 52.26 58.08 L 52.01 58.21 "
    "L 51.75 58.33 L 51.49 58.44 L 51.23 58.54 L 50.95 58.64 L 50.68 58.73 L 50.40 58.81 "
    "L 50.11 58.88 L 49.82 58.94 L 49.53 58.99 L 49.23 59.03 L 48.93 59.07 L 48.63 59.09 "
    "L 48.32 59.11 L 48.01 59.11 L 47.70 59.10 L 47.39 59.09 L 47.08 59.06 L 46.76 59.02 "
    "L 46.45 58.98 L 46.13 58.92 L 45.81 58.85 L 45.50 58.77 L 45.18 58.68 L 44.87 58.57 "
    "L 44.56 58.46 L 44.24 58.34 L 43.94 58.20 L 43.63 58.06 L 43.33 57.90 L 43.03 57.73 "
    "L 42.73 57.55 L 42.44 57.36 L 42.15 57.16 L 41.86 56.95 L 41.59 56.73 L 41.31 56.49 "
    "L 41.05 56.25 L 40.79 56.00 L 40.53 55.73 L 40.28 55.46 L 40.05 55.18 L 39.81 54.88 "
    "L 39.59 54.58 L 39.38 54.27 L 39.17 53.95 L 38.97 53.62 L 38.79 53.28 L 38.61 52.94 "
    "L 38.44 52.58 L 38.29 52.22 L 38.14 51.85 L 38.01 51.48 L 37.88 51.09 L 37.77 50.71 "
    "L 37.67 50.31 L 37.59 49.91 L 37.51 49.50 L 37.45 49.09 L 37.41 48.68 L 37.37 48.26 "
    "L 37.35 47.83 L 37.34 47.41 L 37.35 46.98 L 37.37 46.54 L 37.41 46.11 L 37.46 45.68 "
    "L 37.52 45.24 L 37.60 44.80 L 37.69 44.37 L 37.80 43.93 L 37.92 43.49 L 38.06 43.06 "
    "L 38.22 42.63 L 38.39 42.20 L 38.57 41.77 L 38.77 41.34 L 38.98 40.92 L 39.21 40.51 "
    "L 39.46 40.10 L 39.72 39.69 L 39.99 39.30 L 40.28 38.90 L 40.58 38.52 L 40.90 38.14 "
    "L 41.23 37.77 L 41.58 37.41 L 41.94 37.06 L 42.31 36.72 L 42.70 36.38 L 43.10 36.06 "
    "L 43.51 35.75 L 43.94 35.45 L 44.38 35.17 L 44.83 34.89 L 45.29 34.63 L 45.76 34.39 "
    "L 46.25 34.15 L 46.74 33.94 L 47.25 33.73 L 47.76 33.54 L 48.29 33.37 L 48.82 33.21 "
    "L 49.36 33.07 L 49.91 32.95 L 50.47 32.85 L 51.03 32.76 L 51.60 32.69 L 52.18 32.63 "
    "L 52.76 32.60 L 53.34 32.59 L 53.93 32.59 L 54.52 32.61 L 55.12 32.66 L 55.72 32.72 "
    "L 56.31 32.80 L 56.91 32.90 L 57.51 33.03 L 58.11 33.17 L 58.71 33.33 L 59.31 33.52 "
    "L 59.90 33.72 L 60.50 33.95 L 61.08 34.19 L 61.67 34.46 L 62.24 34.75 L 62.82 35.06 "
    "L 63.38 35.39 L 63.94 35.74 L 64.49 36.10 L 65.03 36.49 L 65.56 36.90 L 66.08 37.33 "
    "L 66.59 37.78 L 67.09 38.25 L 67.58 38.74 L 68.05 39.25 L 68.51 39.77 L 68.96 40.32 "
    "L 69.39 40.88 L 69.80 41.45 L 70.20 42.05 L 70.58 42.66 L 70.95 43.29 L 71.29 43.94 "
    "L 71.62 44.59 L 71.92 45.27 L 72.21 45.96 L 72.48 46.66 L 72.72 47.37 L 72.94 48.10 "
    "L 73.14 48.84 L 73.32 49.58 L 73.47 50.34 L 73.60 51.11 L 73.71 51.89 L 73.79 52.68 "
    "L 73.84 53.47 L 73.87 54.27 L 73.88 55.08 L 73.86 55.89 L 73.81 56.70 L 73.73 57.52 "
    "L 73.63 58.34 L 73.50 59.16 L 73.34 59.98 L 73.15 60.81 L 72.94 61.63 L 72.70 62.45 "
    "L 72.43 63.26 L 72.13 64.08 L 71.80 64.88 L 71.45 65.68 L 71.06 66.48 L 70.65 67.27 "
    "L 70.21 68.05 L 69.74 68.81 L 69.25 69.57 L 68.72 70.32 L 68.17 71.05 L 67.59 71.77 "
    "L 66.99 72.48 L 66.36 73.17 L 65.70 73.84 L 65.01 74.50 L 64.30 75.14 L 63.57 75.76 "
    "L 62.81 76.35 L 62.02 76.93 L 61.22 77.49 L 60.39 78.02 L 59.53 78.53 L 58.66 79.01 "
    "L 57.76 79.47 L 56.85 79.90 L 55.91 80.30 L 54.96 80.68 L 53.99 81.02 L 53.00 81.34 "
    "L 51.99 81.63 L 50.97 81.88 L 49.93 82.11 L 48.88 82.30 L 47.82 82.46 L 46.75 82.58 "
    "L 45.66 82.67 L 44.57 82.73 L 43.47 82.75 L 42.36 82.73 L 41.24 82.68 L 40.12 82.59 "
    "L 39.00 82.47 L 37.87 82.30 L 36.74 82.10 L 35.61 81.86 L 34.49 81.58 L 33.36 81.27 "
    "L 32.24 80.91 L 31.12 80.52 L 30.01 80.09 L 28.91 79.62 L 27.81 79.11 L 26.73 78.56 "
    "L 25.65 77.97 L 24.59 77.34 L 23.54 76.68 L 22.51 75.98 L 21.50 75.24 L 20.50 74.46 "
    "L 19.52 73.64 L 18.58 72.78 L 17.68 71.87 L 16.83 70.91 L 16.03 69.90 L 15.27 68.85 "
    "L 14.57 67.77 L 13.92 66.66 L 13.32 65.51 L 12.76 64.34 L 12.26 63.14 L 11.80 61.93 "
    "L 11.39 60.69 L 11.03 59.44 L 10.71 58.18 L 10.43 56.91 L 10.19 55.62 L 10.00 54.33 "
    "L 9.83 53.03 L 9.71 51.73 L 9.61 50.42 L 9.55 49.11 L 9.52 47.79 L 9.51 46.47 L 9.53 45.15 "
    "L 9.57 43.81 L 9.63 42.47 L 9.71 41.13 L 9.81 39.77 L 9.92 38.40 L 10.06 37.02 L 9.96 38.41 "
    "L 9.93 39.80 L 9.98 41.19 L 10.10 42.56 L 10.29 43.92 L 10.55 45.27 L 10.86 46.59 L 11.23 47.89 "
    "L 11.66 49.16 L 12.14 50.40 L 12.67 51.60 L 13.24 52.78 L 13.85 53.91 L 14.49 55.02 L 15.17 56.08 "
    "L 15.88 57.10 L 16.61 58.09 L 17.36 59.04 L 18.12 59.95 L 18.91 60.83 L 19.70 61.67 L 20.49 62.48 "
    "L 21.30 63.25 L 22.10 64.00 L 22.90 64.71 L 23.70 65.40 L 24.50 66.07 L 25.28 66.72 L 26.06 67.35 "
    "L 26.83 67.97 L 27.60 68.57 L 28.38 69.14 L 29.18 69.68 L 29.99 70.18 L 30.80 70.66 L 31.63 71.11 "
    "L 32.46 71.52 L 33.30 71.91 L 34.14 72.27 L 34.99 72.59 L 35.84 72.89 L 36.70 73.15 L 37.55 73.39 "
    "L 38.41 73.60 L 39.27 73.77 L 40.12 73.92 L 40.97 74.04 L 41.82 74.13 L 42.67 74.19 L 43.51 74.23 "
    "L 44.34 74.24 L 45.17 74.22 L 45.99 74.17 L 46.80 74.10 L 47.60 74.00 L 48.40 73.88 L 49.18 73.73 "
    "L 49.95 73.56 L 50.71 73.36 L 51.46 73.14 L 52.19 72.90 L 52.91 72.64 L 53.61 72.36 L 54.30 72.06 "
    "L 54.98 71.73 L 55.64 71.39 L 56.28 71.03 L 56.90 70.65 L 57.51 70.26 L 58.10 69.85 L 58.67 69.42 "
    "L 59.22 68.98 L 59.76 68.52 L 60.27 68.05 L 60.77 67.57 L 61.24 67.08 L 61.70 66.57 L 62.13 66.06 "
    "L 62.55 65.53 L 62.94 65.00 L 63.32 64.45 L 63.67 63.90 L 64.01 63.35 L 64.32 62.78 L 64.61 62.22 "
    "L 64.88 61.64 L 65.13 61.07 L 65.36 60.49 L 65.57 59.90 L 65.76 59.32 L 65.92 58.73 L 66.07 58.15 "
    "L 66.20 57.56 L 66.31 56.98 L 66.39 56.39 L 66.46 55.81 L 66.51 55.23 L 66.54 54.66 L 66.55 54.08 "
    "L 66.54 53.52 L 66.52 52.95 L 66.47 52.40 L 66.41 51.85 L 66.33 51.30 L 66.24 50.77 L 66.12 50.24 "
    "L 66.00 49.72 L 65.85 49.20 L 65.69 48.70 L 65.52 48.20 L 65.33 47.72 L 65.13 47.25 L 64.91 46.78 "
    "L 64.68 46.33 L 64.44 45.89 L 64.18 45.46 L 63.92 45.04 L 63.64 44.63 L 63.35 44.24 L 63.06 43.86 "
    "L 62.75 43.49 L 62.43 43.13 L 62.10 42.79 L 61.77 42.46 L 61.43 42.15 L 61.08 41.84 L 60.72 41.56 "
    "L 60.36 41.28 L 59.99 41.02 L 59.62 40.78 L 59.24 40.55 L 58.86 40.33 L 58.47 40.12 L 58.08 39.93 "
    "L 57.69 39.76 L 57.29 39.60 L 56.90 39.45 L 56.50 39.32 L 56.10 39.20 L 55.70 39.09 L 55.30 39.00 "
    "L 54.90 38.92 L 54.51 38.86 L 54.11 38.81 L 53.71 38.77 L 53.32 38.75 L 52.93 38.73 L 52.54 38.73 "
    "L 52.16 38.75 L 51.78 38.77 L 51.40 38.81 L 51.03 38.86 L 50.66 38.92 L 50.30 38.99 L 49.94 39.08 "
    "L 49.59 39.17 L 49.25 39.27 L 48.91 39.39 L 48.57 39.51 L 48.25 39.65 L 47.93 39.79 L 47.62 39.94 "
    "L 47.32 40.11 L 47.02 40.28 L 46.73 40.45 L 46.45 40.64 L 46.18 40.83 L 45.92 41.03 L 45.66 41.24 "
    "L 45.42 41.45 L 45.18 41.67 L 44.95 41.90 L 44.73 42.13 L 44.52 42.36 L 44.32 42.60 L 44.13 42.85 "
    "L 43.95 43.10 L 43.78 43.35 L 43.62 43.61 L 43.47 43.86 L 43.32 44.13 L 43.19 44.39 L 43.07 44.66 "
    "L 42.95 44.92 L 42.85 45.19 L 42.75 45.46 L 42.67 45.74 L 42.59 46.01 L 42.52 46.28 L 42.47 46.55 "
    "L 42.42 46.82 L 42.38 47.09 L 42.35 47.36 L 42.33 47.63 L 42.31 47.90 L 42.31 48.16 L 42.31 48.43 "
    "L 42.33 48.69 L 42.35 48.94 L 42.38 49.20 L 42.42 49.45 L 42.46 49.70 L 42.51 49.95 L 42.57 50.19 "
    "L 42.64 50.42 L 42.71 50.66 L 42.80 50.89 L 42.88 51.11 L 42.98 51.33 L 43.08 51.55 L 43.18 51.76 "
    "L 43.30 51.96 L 43.41 52.16 L 43.54 52.36 L 43.67 52.55 L 43.80 52.73 L 43.94 52.91 L 44.08 53.08 "
    "L 44.23 53.24 L 44.38 53.40 L 44.54 53.56 L 44.69 53.70 L 44.86 53.84 L 45.02 53.98 L 45.19 54.11 "
    "L 45.36 54.23 L 45.54 54.34 L 45.71 54.45 L 45.89 54.56 L 46.07 54.65 L 46.25 54.74 L 46.43 54.82 "
    "L 46.62 54.90 L 46.80 54.97 L 46.99 55.03 L 47.17 55.09 L 47.36 55.14 L 47.55 55.19 L 47.73 55.22 "
    "L 47.92 55.26 L 48.10 55.28 L 48.29 55.30 L 48.47 55.31 L 48.66 55.32 L 48.84 55.32 L 49.02 55.32 "
    "L 49.20 55.31 L 49.38 55.29 L 49.55 55.27 L 49.73 55.25 L 49.90 55.21 L 50.07 55.18 L 50.23 55.14 "
    "L 50.40 55.09 L 50.56 55.04 L 50.71 54.98 L 50.87 54.92 L 51.02 54.85 L 51.17 54.78 L 51.32 54.71 "
    "L 51.46 54.63 L 51.60 54.55 L 51.73 54.46 L 51.86 54.38 L 51.99 54.28 L 52.11 54.19 L 52.23 54.09 "
    "L 52.35 53.99 L 52.46 53.88 L 52.56 53.77 L 52.67 53.67 L 52.78 53.56 L 52.89 53.47 L 53.00 53.37 "
    "L 53.11 53.28 L 53.23 53.19 L 53.35 53.10 L 53.47 53.00 L 53.59 52.91 L 53.70 52.81 L 53.82 52.71 "
    "L 53.94 52.61 L 54.06 52.50 L 54.17 52.38 L 54.28 52.27 L 54.39 52.14 L 54.49 52.01 L 54.58 51.88 "
    "L 54.67 51.74 L 54.75 51.60 L 54.82 51.45 L 54.89 51.30 L 54.94 51.14 L 54.98 50.98 L 55.02 50.82 "
    "L 55.04 50.66 L 55.05 50.49 L 55.04 50.33 L 55.03 50.16 L 55.00 50.00 Z"
)

# Brighter foreground stars that gently pulse -- kept few and hand-placed (unlike the generated
# field below) so each one's timing can be tuned to feel organic, not synchronized.
TWINKLE_STARS = [
    # (cx, cy, r, opacity_values, dur, begin)
    (205, 55, 1.5, "0.85;0.25;0.85", "3.2s", "0s"),
    (1060, 80, 1.6, "0.3;0.9;0.3", "4.1s", "0.6s"),
    (1000, 60, 1.3, "0.75;0.2;0.75", "2.7s", "1.1s"),
    (140, 200, 1.4, "0.25;0.8;0.25", "3.6s", "1.7s"),
]


def generate_stars(seed=STAR_SEED, count=STAR_COUNT):
    """A real starfield, not a handful of hand-placed dots. Fixed seed for reproducibility."""
    rng = random.Random(seed)
    stars = []
    for _ in range(count):
        x = round(rng.uniform(10, 1190), 1)
        y = round(rng.uniform(8, 252), 1)
        r = round(rng.choice([0.6, 0.8, 1.0, 1.2, 1.4, 1.6, 1.9]), 2)
        op = round(rng.uniform(0.15, 0.75), 2)
        stars.append((x, y, r, op))
    return stars


def render_star(x, y, r, op):
    return f'    <circle cx="{x}" cy="{y}" r="{r}" opacity="{op}"/>'


def render_twinkle(cx, cy, r, opacity_values, dur, begin):
    return (
        f'    <circle cx="{cx}" cy="{cy}" r="{r}">\n'
        f'      <animate attributeName="opacity" values="{opacity_values}" dur="{dur}" '
        f'begin="{begin}" repeatCount="indefinite"/>\n'
        f"    </circle>"
    )


def build_svg():
    stars_svg = "\n".join(render_star(*s) for s in generate_stars())
    twinkle_svg = "\n".join(render_twinkle(*t) for t in TWINKLE_STARS)
    tx, ty = SPIRAL_TRANSLATE

    return f"""<svg width="{CANVAS_W}" height="{CANVAS_H}" viewBox="0 0 {CANVAS_W} {CANVAS_H}" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Hyperion. The first intent-native operating system.">
  <title>Hyperion</title>
  <defs>
    <!-- The website's own real brand palette (website/app/globals.css, .dark block), not an
         invented one: bg {BG}, accent {ACCENT}, accent-strong {ACCENT_STRONG}, fg {FG},
         fg-muted {FG_MUTED}. -->
    <linearGradient id="titleFill" x1="0%" y1="0%" x2="200%" y2="0%">
      <stop offset="0%" stop-color="{ACCENT_STRONG}"/>
      <stop offset="25%" stop-color="{FG}"/>
      <stop offset="50%" stop-color="{ACCENT}"/>
      <stop offset="75%" stop-color="{FG}"/>
      <stop offset="100%" stop-color="{ACCENT_STRONG}"/>
      <animate attributeName="x1" values="0%;-200%" dur="6s" repeatCount="indefinite"/>
      <animate attributeName="x2" values="200%;0%" dur="6s" repeatCount="indefinite"/>
    </linearGradient>

    <filter id="glow" x="-100%" y="-100%" width="300%" height="300%">
      <feGaussianBlur stdDeviation="3.5" result="blur"/>
      <feMerge>
        <feMergeNode in="blur"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>

    <!-- Soft nebula washes, kept to the corners so they never sit behind the lockup itself and
         cut its contrast. Violet ({NEBULA_VIOLET}) is the same real color PRODUCTION_BOOT_PROMPT.md's M7
         stage-2 display probe proved on a real screen (crates/hyperion-init/src/linux/display_probe.rs). -->
    <radialGradient id="nebulaViolet" cx="120" cy="30" r="360" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="{NEBULA_VIOLET}" stop-opacity="0.4"/>
      <stop offset="100%" stop-color="{NEBULA_VIOLET}" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="nebulaBlue" cx="1100" cy="230" r="380" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="{NEBULA_BLUE}" stop-opacity="0.45"/>
      <stop offset="100%" stop-color="{NEBULA_BLUE}" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="nebulaGold" cx="1080" cy="20" r="300" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="{ACCENT}" stop-opacity="0.16"/>
      <stop offset="100%" stop-color="{ACCENT}" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="nebulaViolet2" cx="90" cy="240" r="300" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="{NEBULA_VIOLET}" stop-opacity="0.3"/>
      <stop offset="100%" stop-color="{NEBULA_VIOLET}" stop-opacity="0"/>
    </radialGradient>
  </defs>

  <rect width="{CANVAS_W}" height="{CANVAS_H}" fill="{BG}"/>
  <rect width="{CANVAS_W}" height="{CANVAS_H}" fill="url(#nebulaViolet)"/>
  <rect width="{CANVAS_W}" height="{CANVAS_H}" fill="url(#nebulaBlue)"/>
  <rect width="{CANVAS_W}" height="{CANVAS_H}" fill="url(#nebulaGold)"/>
  <rect width="{CANVAS_W}" height="{CANVAS_H}" fill="url(#nebulaViolet2)"/>

  <!-- A real starfield ({STAR_COUNT} stars, varied size/opacity, fixed random seed for reproducibility),
       not a handful of hand-placed dots. -->
  <g fill="{FG}">
{stars_svg}
  </g>

  <!-- A few brighter stars that gently twinkle. -->
  <g fill="{FG}">
{twinkle_svg}
  </g>

  <!-- The real website/components/sections/HeroMark.tsx spiral: same path data, same rotation
       origin (50,50), same real rotation timing (its own globals.css: `slow-spin 36s linear
       infinite`) and the same solid accent fill (`text-accent`, {ACCENT}) that component actually
       uses. Positioned via translate+scale only; the rotation is applied around the path's own
       native (50,50) center, so none of its ~200 coordinate pairs needed to change. -->
  <g transform="translate({tx},{ty}) scale({SPIRAL_SCALE})" filter="url(#glow)" fill="{ACCENT}">
    <animateTransform attributeName="transform" type="rotate" from="0 50 50" to="360 50 50" dur="36s" repeatCount="indefinite" additive="sum"/>
    <path d="{HERO_MARK_PATH}"/>
  </g>

  <text x="{LOCKUP_X}" y="{TITLE_Y}" font-family="'Segoe UI', Arial, Helvetica, sans-serif"
        font-size="76" font-weight="700" letter-spacing="6" fill="url(#titleFill)">
    HYPERION
    <animate attributeName="opacity" from="0" to="1" begin="0s" dur="0.9s" fill="freeze"/>
  </text>

  <text x="{LOCKUP_X}" y="{TAGLINE_Y}" font-family="'Segoe UI', Arial, Helvetica, sans-serif"
        font-size="42" fill="{FG}" opacity="0">
    You ask. It understands.
    <animate attributeName="opacity" from="0" to="1" begin="0.5s" dur="0.9s" fill="freeze"/>
  </text>
</svg>
"""


def main():
    out_path = Path(__file__).resolve().parent / "banner.svg"
    out_path.write_text(build_svg())
    print(f"Wrote {out_path}")


if __name__ == "__main__":
    main()
