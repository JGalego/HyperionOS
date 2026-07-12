#!/usr/bin/env bash
# CI-able boot gate for Hyperion's second reference platform (PRODUCTION_BOOT_PROMPT.md M11):
# boots the aarch64 image headless under `qemu-system-aarch64 -M virt`, waits for the same known
# banner string boot-test.sh looks for on the serial console, and exits 0/1 accordingly. Mirrors
# boot-test.sh's structure; the real differences are all in the qemu invocation itself -- direct
# kernel load (no firmware/bootloader stage exists on this path, unlike x86_64's OVMF+GRUB2), a
# separate rootfs.ext2 with no partition table (so `root=/dev/vda` needs no PARTUUID), and the
# aarch64-virt machine's PL011 console (`ttyAMA0`, not `ttyS0`).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
IMAGES_DIR="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output-aarch64/images}"
KERNEL_IMAGE="$IMAGES_DIR/Image"
ROOTFS_IMAGE="$IMAGES_DIR/rootfs.ext2"
# Same oracle string as boot-test.sh: hyperion-init prints this directly to its inherited
# /dev/console (see crates/hyperion-init/src/linux.rs), independent of any getty/inittab setup,
# so it appears here exactly as it does on the x86_64 platform.
EXPECT="${BOOT_TEST_EXPECT:-Humans express goals}"
TIMEOUT_S="${BOOT_TEST_TIMEOUT:-180}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

echo "Booting $KERNEL_IMAGE + $ROOTFS_IMAGE (TCG, timeout ${TIMEOUT_S}s), waiting for \"$EXPECT\" on serial..."

timeout "$TIMEOUT_S" qemu-system-aarch64 \
    -M virt \
    -cpu cortex-a53 \
    -m 2048 \
    -smp 2 \
    -display none \
    -no-reboot \
    -serial file:"$LOG" \
    -kernel "$KERNEL_IMAGE" \
    -append "root=/dev/vda rootwait console=ttyAMA0 init=/hyperion-init ip=dhcp" \
    -drive file="$ROOTFS_IMAGE",if=none,format=raw,id=hd0 \
    -device virtio-blk-device,drive=hd0 \
    -netdev user,id=net0 \
    -device virtio-net-device,netdev=net0 &
QEMU_PID=$!

FOUND=0
for _ in $(seq 1 "$TIMEOUT_S"); do
    if grep -q "$EXPECT" "$LOG" 2>/dev/null; then
        FOUND=1
        break
    fi
    if ! kill -0 "$QEMU_PID" 2>/dev/null; then
        break
    fi
    sleep 1
done

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

echo "--- serial console log ---"
cat "$LOG"
echo "--- end log ---"

if [[ "$FOUND" -eq 1 ]]; then
    echo "PASS: found \"$EXPECT\" within ${TIMEOUT_S}s"
    exit 0
else
    echo "FAIL: \"$EXPECT\" did not appear within ${TIMEOUT_S}s"
    exit 1
fi
