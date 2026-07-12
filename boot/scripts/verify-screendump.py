#!/usr/bin/env python3
"""PRODUCTION_BOOT_PROMPT.md M7 stage 2: verifies a real PPM screenshot (captured by
screendump.py from the actual emulated display, independent of the guest) really shows the exact
three-band color pattern crates/hyperion-init/src/linux/display_probe.rs writes into its real
dumb buffer -- proof the real DRM/KMS mode-set actually reached the screen, not just that the
guest-side ioctls returned success.

Usage: verify-screendump.py <ppm-path>
"""
import sys

# The exact RGB colors display_probe.rs's own BAND_COLORS_BGRX array means, decoded from
# DRM_FORMAT_XRGB8888's real byte order (little-endian 0x00RRGGBB -> bytes [B, G, R, X]).
EXPECTED_BANDS_RGB = [
    (0x6B, 0x2C, 0x4A),  # deep violet
    (0xFF, 0xFF, 0xFF),  # white
    (0x4A, 0x2C, 0x6B),  # deep magenta
]
TOLERANCE = 16


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


def main():
    path = sys.argv[1]
    width, height, pixels = read_ppm(path)
    print(f"real screenshot: {width}x{height}")

    band_height = height // len(EXPECTED_BANDS_RGB)
    all_match = True
    for i, expected in enumerate(EXPECTED_BANDS_RGB):
        sample_y = min(i * band_height + band_height // 2, height - 1)
        sample_x = width // 2
        actual = pixel_at(pixels, width, sample_x, sample_y)
        ok = close_enough(actual, expected)
        all_match = all_match and ok
        status = "OK" if ok else "MISMATCH"
        print(
            f"band {i} (row {sample_y}): expected RGB{expected}, got RGB{actual} -- {status}"
        )

    if all_match:
        print("PASS: the real captured screenshot shows the exact expected three-band pattern")
        sys.exit(0)
    else:
        print("FAIL: the real captured screenshot does not match the expected pattern")
        sys.exit(1)


if __name__ == "__main__":
    main()
