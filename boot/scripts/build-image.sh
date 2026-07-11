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

echo "Overlaying board/hyperion-x86_64 and the Hyperion defconfig onto Buildroot..."
rsync -a --delete "$BOOT_DIR/board/hyperion-x86_64/" "$BUILDROOT_DIR/board/hyperion-x86_64/"
cp "$BOOT_DIR/configs/hyperion_x86_64_efi_defconfig" "$BUILDROOT_DIR/configs/hyperion_x86_64_efi_defconfig"

cd "$BUILDROOT_DIR"
make hyperion_x86_64_efi_defconfig
make

echo "Image built: $BUILDROOT_DIR/output/images/disk.img"
