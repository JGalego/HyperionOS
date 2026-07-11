#!/usr/bin/env bash
# Downloads and extracts the pinned Buildroot release used to build Hyperion's
# boot image. Buildroot itself is treated as a vendored build tool: it is not
# committed to the Hyperion repo (multi-hundred-MB source tree, unrelated
# history) -- this script re-derives it reproducibly instead, the same way
# setup-qemu-toolchain.sh re-derives qemu/OVMF instead of committing binaries.
#
# Safe to both `source` (to pick up $BUILDROOT_DIR) and execute directly --
# the guard below uses `return`, not `exit`, so sourcing it doesn't terminate
# the calling script when Buildroot is already present.
set -euo pipefail

BUILDROOT_VERSION="2026.05"
BUILDROOT_SHA256="9d2f3af10fcac763a61ff6e41894a033f9ecf9267ba13dd0912eedcd3be2b22a"

_hyperion_fetch_buildroot() {
    local script_dir boot_dir tools_dir tarball
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    boot_dir="$(cd "$script_dir/.." && pwd)"
    tools_dir="$boot_dir/.tools"
    BUILDROOT_DIR="$tools_dir/buildroot-$BUILDROOT_VERSION"

    if [[ -d "$BUILDROOT_DIR" ]]; then
        echo "Buildroot $BUILDROOT_VERSION already present at $BUILDROOT_DIR"
        return 0
    fi

    mkdir -p "$tools_dir"
    tarball="$tools_dir/buildroot-$BUILDROOT_VERSION.tar.xz"

    if [[ ! -f "$tarball" ]]; then
        echo "Downloading Buildroot $BUILDROOT_VERSION..."
        curl -fSL -o "$tarball.tmp" "https://buildroot.org/downloads/buildroot-$BUILDROOT_VERSION.tar.xz"
        mv "$tarball.tmp" "$tarball"
    fi

    echo "$BUILDROOT_SHA256  $tarball" | sha256sum -c -

    tar -C "$tools_dir" -xf "$tarball"
    echo "Extracted Buildroot $BUILDROOT_VERSION to $BUILDROOT_DIR"
}

_hyperion_fetch_buildroot
export BUILDROOT_DIR
