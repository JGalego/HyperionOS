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
echo "Cross-compiling hyperion-init, hyperion-console and the supervised services (static, x86_64-unknown-linux-musl)..."
# Every target is named explicitly. `--bin` is not scoped to the `-p` it follows: one `--bin`
# anywhere filters the whole invocation, so listing only the two services silently built only
# those two and left hyperion-init unbuilt -- the failure surfaced later as a `cp: cannot stat`
# on a path the build was supposed to have produced.
( cd "$REPO_ROOT" && cargo build --release --target x86_64-unknown-linux-musl \
    -p hyperion-init --bin hyperion-init \
    -p hyperion-console --bin hyperion-console \
    -p hyperion-observability --bin hyperion-observability-service \
    -p hyperion-explainability --bin hyperion-explainability-service )
MUSL_RELEASE="$REPO_ROOT/target/x86_64-unknown-linux-musl/release"
HYPERION_INIT_BIN="$MUSL_RELEASE/hyperion-init"
HYPERION_CONSOLE_BIN="$MUSL_RELEASE/hyperion-console"

echo "Overlaying board/hyperion-x86_64 and the Hyperion defconfig onto Buildroot..."
rsync -a --delete "$BOOT_DIR/board/hyperion-x86_64/" "$BUILDROOT_DIR/board/hyperion-x86_64/"
cp "$BOOT_DIR/configs/hyperion_x86_64_efi_defconfig" "$BUILDROOT_DIR/configs/hyperion_x86_64_efi_defconfig"

# The overlay lives entirely inside the (gitignored) Buildroot copy, populated fresh from the
# just-built binaries each run -- rsync --delete above would otherwise wipe it if it lived under
# the tracked boot/board/hyperion-x86_64 source, which has no rootfs-overlay/ of its own.
OVERLAY_DIR="$BUILDROOT_DIR/board/hyperion-x86_64/rootfs-overlay"
mkdir -p "$OVERLAY_DIR" "$OVERLAY_DIR/usr/bin"
cp "$HYPERION_INIT_BIN" "$OVERLAY_DIR/hyperion-init"
chmod 755 "$OVERLAY_DIR/hyperion-init"
cp "$HYPERION_CONSOLE_BIN" "$OVERLAY_DIR/usr/bin/hyperion-console"
chmod 755 "$OVERLAY_DIR/usr/bin/hyperion-console"

# The two representative Phase 2-10 supervised services (M4/M5). `hyperion-init` already looks for
# these at /usr/lib/hyperion/services and skips any that are absent with a clear warning -- which
# is exactly what every boot did until now, logging `skipping "observability"` and
# `skipping "explainability"` on an image that contained neither. The mechanism was proven in
# tests; the booted system never exercised it. Copying them in is the "purely mechanical follow-on"
# `phase_2_10_service_specs`'s own doc comment describes, and it makes the boot test real evidence
# that supervision works on the real image rather than only under `cargo test`.
mkdir -p "$OVERLAY_DIR/usr/lib/hyperion/services"
for service in hyperion-observability-service hyperion-explainability-service; do
    cp "$MUSL_RELEASE/$service" "$OVERLAY_DIR/usr/lib/hyperion/services/$service"
    chmod 755 "$OVERLAY_DIR/usr/lib/hyperion/services/$service"
done

cd "$BUILDROOT_DIR"
make hyperion_x86_64_efi_defconfig
make

echo "Image built: $BUILDROOT_DIR/output/images/disk.img"
