#!/usr/bin/env bash
# PRODUCTION_BOOT_PROMPT.md M13: docs/41 Phase 10's literal exit criterion, proven for real --
# "a staged update applied to a real running booted system and rolled back without data loss."
# Boots the real aarch64 image (direct kernel load, so the trigger flag can go straight on QEMU's
# own -append cmdline -- see this test's own comment on why the x86_64 platform, which boots via a
# GRUB-embedded cmdline instead, isn't used here) with a real, dedicated, pre-formatted ext4 data
# disk attached and `hyperion.run_update_test=1` on the kernel cmdline, which
# crates/hyperion-init/src/linux/update_probe.rs reads from the real /proc/cmdline to opt into
# running its own real apply_update -> mutate -> update_rollback -> verify sequence, entirely
# self-contained within this one boot (no crash/reboot semantics needed, unlike M6's own
# storage-crash-test.sh).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
IMAGES_DIR="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output-aarch64/images}"
TIMEOUT_S="${UPDATE_TEST_TIMEOUT:-120}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

DATA_IMG="$(mktemp --suffix=.img)"
"$SCRIPT_DIR/create-data-disk.sh" "$DATA_IMG" 64M >/dev/null
LOG="$(mktemp)"
trap 'rm -f "$LOG" "$DATA_IMG"' EXIT

echo "Booting $IMAGES_DIR (aarch64, TCG) with a real data disk + hyperion.run_update_test=1..."

timeout "$TIMEOUT_S" qemu-system-aarch64 \
    -M virt \
    -cpu cortex-a53 \
    -m 2048 \
    -smp 2 \
    -display none \
    -no-reboot \
    -serial file:"$LOG" \
    -kernel "$IMAGES_DIR/Image" \
    -append "root=/dev/vda rootwait console=ttyAMA0 init=/hyperion-init hyperion.run_update_test=1 ip=dhcp" \
    -drive file="$DATA_IMG",if=none,format=raw,id=hd1,cache=none \
    -device virtio-blk-device,drive=hd1 \
    -drive file="$IMAGES_DIR/rootfs.ext2",if=none,format=raw,id=hd0 \
    -device virtio-blk-device,drive=hd0 \
    -netdev user,id=net0 \
    -device virtio-net-device,netdev=net0 &
QEMU_PID=$!

FOUND=0
for _ in $(seq 1 "$TIMEOUT_S"); do
    if grep -q "UPDATE_TEST: PASS\|UPDATE_TEST: FAIL" "$LOG" 2>/dev/null; then
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

if [[ "$FOUND" -eq 1 ]] && grep -q "UPDATE_TEST: PASS" "$LOG"; then
    echo "PASS: real update+rollback verified, no data loss"
    exit 0
else
    echo "FAIL: real update+rollback did not report PASS within ${TIMEOUT_S}s"
    exit 1
fi
