#!/usr/bin/env bash
# PRODUCTION_BOOT_PROMPT.md M13: packages the already-built, already boot-tested x86_64/aarch64
# images into a signed, versioned release -- a real BLAKE3 hash + real Ed25519 signature (M9's
# real device keystore) over each image's own bytes, written as a `.release.json` manifest
# alongside it, then both images + manifests copied into a single, clearly labeled release
# directory. This is the "signed, versioned, dd-able USB image... published as the actual release
# artifact" M13's own text asks for -- it does NOT write anything to a real physical device itself
# (see this repo's own standing instruction: that one step always needs an explicit, separate
# go-ahead with the exact target verified first). Verifying a produced manifest, or actually
# `dd`-ing it to a real drive, is this script's caller's job, not this script's.
#
# Usage: package-release.sh [version]  (defaults to `git describe --tags --always --dirty`)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$BOOT_DIR/.." && pwd)"
VERSION="${1:-$(cd "$REPO_ROOT" && git describe --tags --always --dirty)}"
KEYSTORE_PATH="$BOOT_DIR/.tools/release-keystore/device.key"
RELEASE_DIR="$BOOT_DIR/.tools/release/$VERSION"

X86_64_IMG="$BOOT_DIR/.tools/buildroot-2026.05/output/images/disk.img"
AARCH64_KERNEL="$BOOT_DIR/.tools/buildroot-2026.05/output-aarch64/images/Image"
AARCH64_ROOTFS="$BOOT_DIR/.tools/buildroot-2026.05/output-aarch64/images/rootfs.ext2"

for f in "$X86_64_IMG" "$AARCH64_KERNEL" "$AARCH64_ROOTFS"; do
    if [[ ! -f "$f" ]]; then
        echo "missing $f -- build both platform images first (build-image.sh, build-image-aarch64.sh)" >&2
        exit 1
    fi
done

mkdir -p "$RELEASE_DIR"
mkdir -p "$(dirname "$KEYSTORE_PATH")"

echo "Packaging release $VERSION into $RELEASE_DIR..."

cp "$X86_64_IMG" "$RELEASE_DIR/hyperion-x86_64-$VERSION.img"
cp "$AARCH64_KERNEL" "$RELEASE_DIR/hyperion-aarch64-$VERSION-Image"
cp "$AARCH64_ROOTFS" "$RELEASE_DIR/hyperion-aarch64-$VERSION-rootfs.ext2"

( cd "$REPO_ROOT" && cargo run --release -p hyperion-release-gate --bin sign-release -- \
    "$RELEASE_DIR/hyperion-x86_64-$VERSION.img" "$KEYSTORE_PATH" "$VERSION" "x86_64" )
( cd "$REPO_ROOT" && cargo run --release -p hyperion-release-gate --bin sign-release -- \
    "$RELEASE_DIR/hyperion-aarch64-$VERSION-Image" "$KEYSTORE_PATH" "$VERSION" "aarch64-kernel" )
( cd "$REPO_ROOT" && cargo run --release -p hyperion-release-gate --bin sign-release -- \
    "$RELEASE_DIR/hyperion-aarch64-$VERSION-rootfs.ext2" "$KEYSTORE_PATH" "$VERSION" "aarch64-rootfs" )

echo ""
echo "=== Release $VERSION packaged and signed ==="
ls -la "$RELEASE_DIR"
echo ""
echo "Verifying key for this device's real signing identity:"
python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['verifying_key'])" \
    "$RELEASE_DIR/hyperion-x86_64-$VERSION.img.release.json"
echo ""
echo "This is a real, signed, versioned release artifact -- nothing here has written to any real"
echo "physical device. hyperion-x86_64-$VERSION.img is a plain, complete GPT disk image (real EFI"
echo "System Partition + GRUB2 + real root partition) -- write it to a real USB drive with Balena"
echo "Etcher (recommended: writes a raw .img natively, hides system drives, verifies the write"
echo "afterward) or a careful \`dd if=... of=/dev/sdX\`. Either way, that step needs its own explicit"
echo "confirmation of the exact target device before running, every time."
