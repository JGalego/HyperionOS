#!/usr/bin/env bash
# CI-able boot gate (PRODUCTION_BOOT_PROMPT.md section 5): boots the image
# headless, waits for a known banner string to appear on the serial console
# within a timeout, and exits 0/1 accordingly. No KVM in this environment, so
# the timeout is generous for TCG software emulation.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DISK_IMG="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output/images/disk.img}"
# NB: must not appear in the kernel cmdline itself (e.g. plain "hyperion-init"
# would false-positive-match "init=/hyperion-init" echoed by the kernel at
# boot, long before the real hyperion-init binary has run) -- pick a string
# that can only come from the program's own output.
EXPECT="${BOOT_TEST_EXPECT:-Humans express goals}"
TIMEOUT_S="${BOOT_TEST_TIMEOUT:-180}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

VARS_COPY="$(mktemp --suffix=.fd)"
LOG="$(mktemp)"
trap 'rm -f "$VARS_COPY" "$LOG"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$VARS_COPY"

echo "Booting $DISK_IMG (TCG, timeout ${TIMEOUT_S}s), waiting for \"$EXPECT\" on serial..."

timeout "$TIMEOUT_S" qemu-system-x86_64 \
    -M pc \
    -m 2048 \
    -smp 2 \
    -display none \
    -no-reboot \
    -serial file:"$LOG" \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$VARS_COPY" \
    -drive file="$DISK_IMG",if=virtio,format=raw \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 &
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
