#!/usr/bin/env bash
# Builds the Hyperion boot image with Buildroot: fetches Buildroot if needed,
# overlays Hyperion's board config on top of it, and runs the build. Output
# lands in $BUILDROOT_DIR/output/images/disk.img.
set -euo pipefail

# This dev environment is WSL2, which appends the (translated) Windows PATH --
# entries like "/mnt/c/Program Files/.../bin" contain spaces. Buildroot's
# top-level Makefile explicitly refuses to run if PATH contains whitespace, so
# strip any such entries here rather than touching the user's shell profile.
CLEAN_PATH=""
IFS=':' read -ra _path_parts <<< "$PATH"
for _p in "${_path_parts[@]}"; do
    case "$_p" in
        *[[:space:]]*) ;;
        *) CLEAN_PATH="${CLEAN_PATH:+$CLEAN_PATH:}$_p" ;;
    esac
done
export PATH="$CLEAN_PATH"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck source=./fetch-buildroot.sh
source "$SCRIPT_DIR/fetch-buildroot.sh"

REPO_ROOT="$(cd "$BOOT_DIR/.." && pwd)"
echo "Cross-compiling hyperion-init (static, x86_64-unknown-linux-musl)..."
( cd "$REPO_ROOT" && cargo build -p hyperion-init --release --target x86_64-unknown-linux-musl )
HYPERION_INIT_BIN="$REPO_ROOT/target/x86_64-unknown-linux-musl/release/hyperion-init"

echo "Overlaying board/hyperion-x86_64 and the Hyperion defconfig onto Buildroot..."
rsync -a --delete "$BOOT_DIR/board/hyperion-x86_64/" "$BUILDROOT_DIR/board/hyperion-x86_64/"
cp "$BOOT_DIR/configs/hyperion_x86_64_efi_defconfig" "$BUILDROOT_DIR/configs/hyperion_x86_64_efi_defconfig"

# The overlay lives entirely inside the (gitignored) Buildroot copy, populated fresh from the
# just-built binary each run -- rsync --delete above would otherwise wipe it if it lived under
# the tracked boot/board/hyperion-x86_64 source, which has no rootfs-overlay/ of its own.
OVERLAY_DIR="$BUILDROOT_DIR/board/hyperion-x86_64/rootfs-overlay"
mkdir -p "$OVERLAY_DIR"
cp "$HYPERION_INIT_BIN" "$OVERLAY_DIR/hyperion-init"
chmod 755 "$OVERLAY_DIR/hyperion-init"

cd "$BUILDROOT_DIR"
make hyperion_x86_64_efi_defconfig
make

echo "Image built: $BUILDROOT_DIR/output/images/disk.img"
