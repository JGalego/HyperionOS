#!/usr/bin/env bash
# Builds the Hyperion aarch64 boot image with Buildroot (docs/998-roadmap.md M11): fetches
# Buildroot if needed, overlays Hyperion's aarch64 board config on top of it, and runs the build.
# Output lands in $BUILDROOT_DIR/output-aarch64/images/{Image,rootfs.ext2}. Mirrors build-image.sh's
# structure exactly, with one deliberate difference: a dedicated `O=output-aarch64` build output
# directory, distinct from build-image.sh's own default (implicit) `output/`. The two builds are
# for different target architectures and each needs its own from-source host cross-toolchain
# (gcc-initial/gcc-final/binutils, since BR2_TOOLCHAIN_BUILDROOT_GLIBC builds one rather than using
# a system package) -- sharing one output directory made the aarch64 build's kernel-config step
# reach for `aarch64-buildroot-linux-gnu-gcc`, silently found the x86_64 build's already-built
# host-gcc-final instead, and failed with "compiler not found" once the mismatch surfaced. See that
# script's own comments for the WSL2-PATH-whitespace workaround shared here unchanged.
set -euo pipefail

CLEAN_PATH=""
IFS=':' read -ra _path_parts <<< "$PATH"
for _p in "${_path_parts[@]}"; do
    case "$_p" in
        *[[:space:]]*) ;;
        *) CLEAN_PATH="${CLEAN_PATH:+$CLEAN_PATH:}$_p" ;;
    esac
done
export PATH="$CLEAN_PATH"

# Defensive: an inherited LD_LIBRARY_PATH (e.g. left over in a dev shell from manually testing the
# aarch64 cross-toolchain) can carry a trailing-colon/cwd-implying entry that Buildroot's own
# dependencies.mk pre-flight check explicitly refuses to run under. Nothing this script itself
# invokes before the scoped cargo build below needs any LD_LIBRARY_PATH at all.
unset LD_LIBRARY_PATH

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck source=./fetch-buildroot.sh
source "$SCRIPT_DIR/fetch-buildroot.sh"

REPO_ROOT="$(cd "$BOOT_DIR/.." && pwd)"

"$SCRIPT_DIR/setup-aarch64-toolchain.sh"

echo "Cross-compiling hyperion-init and hyperion-console (static, aarch64-unknown-linux-musl)..."
# Scoped to just this invocation -- the cross-gcc is only needed to *link* the Rust binaries
# above; Buildroot's own `make` below uses its own internal toolchain and, unlike qemu/cargo,
# explicitly refuses to run at all if it inherits a stray LD_LIBRARY_PATH (its dependencies.mk
# pre-flight check rejects any trailing-colon/cwd-implying entry), so it must not leak past here.
( cd "$REPO_ROOT" && \
  PATH="$BOOT_DIR/.tools/aarch64-cross-root/usr/bin:$PATH" \
  LD_LIBRARY_PATH="$BOOT_DIR/.tools/aarch64-cross-root/usr/lib/x86_64-linux-gnu" \
  cargo build -p hyperion-init -p hyperion-console --release --target aarch64-unknown-linux-musl )
HYPERION_INIT_BIN="$REPO_ROOT/target/aarch64-unknown-linux-musl/release/hyperion-init"
HYPERION_CONSOLE_BIN="$REPO_ROOT/target/aarch64-unknown-linux-musl/release/hyperion-console"

echo "Overlaying board/hyperion-aarch64 and the Hyperion defconfig onto Buildroot..."
rsync -a --delete "$BOOT_DIR/board/hyperion-aarch64/" "$BUILDROOT_DIR/board/hyperion-aarch64/"
cp "$BOOT_DIR/configs/hyperion_aarch64_virt_defconfig" "$BUILDROOT_DIR/configs/hyperion_aarch64_virt_defconfig"

# Build-time-only staging, same reasoning as build-image.sh's own OVERLAY_DIR comment: this lives
# entirely inside the gitignored Buildroot copy so rsync --delete above never touches it.
OVERLAY_DIR="$BUILDROOT_DIR/board/hyperion-aarch64/rootfs-overlay"
mkdir -p "$OVERLAY_DIR" "$OVERLAY_DIR/usr/bin"
cp "$HYPERION_INIT_BIN" "$OVERLAY_DIR/hyperion-init"
chmod 755 "$OVERLAY_DIR/hyperion-init"
cp "$HYPERION_CONSOLE_BIN" "$OVERLAY_DIR/usr/bin/hyperion-console"
chmod 755 "$OVERLAY_DIR/usr/bin/hyperion-console"

OUTPUT_DIR="$BUILDROOT_DIR/output-aarch64"
cd "$BUILDROOT_DIR"
make O="$OUTPUT_DIR" hyperion_aarch64_virt_defconfig
make O="$OUTPUT_DIR"

echo "Image built: $OUTPUT_DIR/images/Image (kernel), $OUTPUT_DIR/images/rootfs.ext2 (rootfs)"
