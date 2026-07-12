#!/usr/bin/env bash
# Provisions qemu-system-x86_64 + OVMF UEFI firmware, and qemu-system-aarch64 (PRODUCTION_BOOT_PROMPT.md
# M11's second reference platform -- no firmware needed there, since board/hyperion-aarch64 boots
# via direct kernel load, not UEFI), all without root.
#
# This dev environment has no passwordless sudo, so `apt-get install` is not
# an option. `apt-get download` (unlike `install`) needs no privilege -- it
# just fetches the .deb into the cwd -- and `dpkg -x` extracts a .deb's
# contents into an arbitrary directory without touching the system package
# database. Since the packages come from this exact host's configured Debian
# release, the extracted binaries are ABI-compatible with the host's existing
# shared libraries; only a handful of libs Debian ships in separate runtime
# packages (capstone, fdt, pmem, slirp, vdeplug, uring, fuse3, aio, brlapi,
# cacard, execs, spice-server, usbredirparser) need extracting too. Safe to
# re-run: skips work if already provisioned.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PREFIX="$BOOT_DIR/.tools/qemu-root"
STAMP="$PREFIX/.provisioned"

if [[ -f "$STAMP" ]]; then
    echo "qemu toolchain already provisioned at $PREFIX"
    exit 0
fi

PACKAGES=(
    qemu-system-x86 qemu-system-arm qemu-system-common qemu-system-data qemu-utils
    ovmf seabios ipxe-qemu
    libcapstone4 libfdt1 libpmem1 libslirp0 libvdeplug2 liburing2 libfuse3-3 libaio1
    libndctl6 libdaxctl1 libbrlapi0.8 libcacard0 libexecs0 libspice-server1 libusbredirparser1
)

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Downloading ${#PACKAGES[@]} packages via apt-get download (no root required)..."
(cd "$WORK" && apt-get download "${PACKAGES[@]}")

mkdir -p "$PREFIX"
for deb in "$WORK"/*.deb; do
    dpkg -x "$deb" "$PREFIX"
done

touch "$STAMP"
echo "Provisioned qemu toolchain at $PREFIX"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"
qemu-system-x86_64 --version
export PATH="$PREFIX/usr/bin:$PATH"
export LD_LIBRARY_PATH="$PREFIX/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
qemu-system-aarch64 --version
