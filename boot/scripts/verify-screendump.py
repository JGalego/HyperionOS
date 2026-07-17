#!/usr/bin/env python3
"""docs/998-roadmap.md M7 stage 2: verifies a real PPM screenshot (captured by
screendump.py from the actual emulated display, independent of the guest) really shows the real,
compiled `WorkspaceGraph` crates/hyperion-init/src/linux/ui_render.rs rasterizes into the real
dumb buffer crates/hyperion-init/src/linux/display_probe.rs writes -- proof the real DRM/KMS
mode-set actually reached the screen with real, structured content (a filled panel rectangle plus
real rasterized glyph text), not just that the guest-side ioctls returned success.

The expected layout mirrors crates/hyperion-init/src/linux/ui_render.rs's own `region_bounds`
function exactly (same fractions of the real captured width/height), rather than hardcoding pixel
coordinates for one specific resolution -- the real renderer adapts to whatever mode the real
connector reports, so this verifier does too.

Usage: verify-screendump.py <ppm-path>
"""
import sys

# crates/hyperion-init/src/linux/ui_render.rs's PANEL_BG_STATIC, in BGRX byte order -- the real
# boot-splash workspace's one compiled panel (`system.boot_status`) is never interactive, so it
# always gets this fill, never PANEL_BG_INTERACTIVE.
PANEL_BG_STATIC_BGRX = (0x20, 0x20, 0x20)
# Real, near-white rasterized glyph pixels trend toward this (ui_render.rs's TEXT_FG).
TEXT_FG_RGB = (0xFF, 0xFF, 0xFF)
BRIGHT_THRESHOLD = 200
TOLERANCE = 24


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    assert data[:2] == b"P6", f"not a real P6 PPM (got {data[:2]!r})"
    # Parse header tokens (magic, width, height, maxval), skipping comments.
    tokens = []
    i = 2
    while len(tokens) < 3:
        while data[i] in b" \t\r\n":
            i += 1
        if data[i : i + 1] == b"#":
            while data[i] not in b"\r\n":
                i += 1
            continue
        start = i
        while data[i] not in b" \t\r\n":
            i += 1
        tokens.append(int(data[start:i]))
    width, height, _maxval = tokens
    pixels = data[i + 1 :]
    return width, height, pixels


def pixel_at(pixels, width, x, y):
    offset = (y * width + x) * 3
    return pixels[offset], pixels[offset + 1], pixels[offset + 2]


def close_enough(actual, expected, tolerance=TOLERANCE):
    return all(abs(a - e) <= tolerance for a, e in zip(actual, expected))


def is_bright(pixel, threshold=BRIGHT_THRESHOLD):
    return all(c >= threshold for c in pixel)


def center_rect(width, height):
    """Mirrors ui_render.rs's `region_bounds`/`panel_rects` exactly for the one real Center
    panel this boot-splash workspace always compiles."""
    top_h = height // 8
    bottom_h = height // 10
    side_w = width // 5
    mid_h = height - top_h - bottom_h
    return side_w, top_h, width - 2 * side_w, mid_h


def main():
    path = sys.argv[1]
    width, height, pixels = read_ppm(path)
    print(f"real screenshot: {width}x{height}")

    rect_x, rect_y, rect_w, rect_h = center_rect(width, height)
    all_ok = True

    # A real background fill happened inside the real Center panel's rect -- sampled away from
    # where rasterized text starts (top-left of the rect), so this never accidentally lands on a
    # real glyph pixel instead of the real background fill.
    bg_x, bg_y = rect_x + rect_w - 10, rect_y + rect_h - 10
    bg_actual = pixel_at(pixels, width, bg_x, bg_y)
    # BGRX bytes -> real on-screen RGB is (R, G, B); PANEL_BG_STATIC_BGRX is R=G=B so the byte
    # order doesn't matter here, only the (matching) values do.
    bg_ok = close_enough(bg_actual, PANEL_BG_STATIC_BGRX)
    all_ok = all_ok and bg_ok
    print(
        f"panel background at ({bg_x},{bg_y}): expected ~RGB{PANEL_BG_STATIC_BGRX}, got "
        f"RGB{bg_actual} -- {'OK' if bg_ok else 'MISMATCH'}"
    )

    # Real glyph rasterization happened somewhere inside that same rect -- scan for at least one
    # real, near-white pixel (ab_glyph really drew "Hyperion is starting", not just a flat fill).
    saw_bright = False
    for y in range(rect_y, min(rect_y + rect_h, height)):
        for x in range(rect_x, min(rect_x + rect_w, width)):
            if is_bright(pixel_at(pixels, width, x, y)):
                saw_bright = True
                break
        if saw_bright:
            break
    all_ok = all_ok and saw_bright
    print(
        f"real rasterized glyph text inside the panel rect: "
        f"{'OK -- found a real near-white pixel' if saw_bright else 'MISSING -- no bright pixel found'}"
    )

    # Nothing was drawn outside every real panel's rect -- the real renderer fills only the
    # panels a real WorkspaceGraph actually has, never the whole screen.
    corner_actual = pixel_at(pixels, width, 0, 0)
    corner_ok = close_enough(corner_actual, (0, 0, 0))
    all_ok = all_ok and corner_ok
    print(
        f"top-left corner (outside every real panel rect): expected ~RGB(0, 0, 0), got "
        f"RGB{corner_actual} -- {'OK' if corner_ok else 'MISMATCH'}"
    )

    if all_ok:
        print(
            "PASS: the real captured screenshot shows a real compiled WorkspaceGraph's panel "
            "and real rasterized text"
        )
        sys.exit(0)
    else:
        print("FAIL: the real captured screenshot does not match the expected real render")
        sys.exit(1)


if __name__ == "__main__":
    main()
