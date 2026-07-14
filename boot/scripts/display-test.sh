#!/usr/bin/env bash
# docs/998-roadmap.md M7 stage 2: real DRM/KMS mode-set, proven for real. Boots with a real
# virtio-gpu-pci display device attached (every other boot script in this repo attaches none, so
# this is the first to actually exercise crates/hyperion-init/src/linux/display_probe.rs), waits
# for its own real DISPLAY: PASS/FAIL marker, then captures the *actual emulated display's* real
# current pixel content via a real `screendump` HMP monitor command -- independent proof, from
# outside the guest entirely, that real pixels are really being displayed, not just that the
# guest-side ioctls happened to return success codes.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DISK_IMG="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output/images/disk.img}"
TIMEOUT_S="${DISPLAY_TEST_TIMEOUT:-180}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

MONITOR_SOCK="$(mktemp -u --suffix=.sock)"
VARS_COPY="$(mktemp --suffix=.fd)"
LOG="$(mktemp)"
PPM_OUT="$(mktemp --suffix=.ppm)"
trap 'rm -f "$MONITOR_SOCK" "$VARS_COPY" "$LOG" "$PPM_OUT"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$VARS_COPY"

echo "Booting $DISK_IMG (TCG) with a real virtio-gpu-pci display device attached..."

timeout "$TIMEOUT_S" qemu-system-x86_64 \
    -M pc \
    -m 2048 \
    -smp 2 \
    -vga none \
    -device virtio-gpu-pci \
    -display none \
    -no-reboot \
    -serial file:"$LOG" \
    -monitor unix:"$MONITOR_SOCK",server=on,wait=off \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$VARS_COPY" \
    -drive file="$DISK_IMG",if=virtio,format=raw \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 &
QEMU_PID=$!

FOUND=0
for _ in $(seq 1 "$TIMEOUT_S"); do
    if grep -q "DISPLAY: PASS\|DISPLAY: FAIL" "$LOG" 2>/dev/null; then
        FOUND=1
        break
    fi
    if ! kill -0 "$QEMU_PID" 2>/dev/null; then
        break
    fi
    sleep 1
done

if [[ "$FOUND" -eq 1 ]] && grep -q "DISPLAY: PASS" "$LOG"; then
    for _ in $(seq 1 10); do
        [[ -S "$MONITOR_SOCK" ]] && break
        sleep 1
    done
    python3 "$SCRIPT_DIR/screendump.py" "$MONITOR_SOCK" "$PPM_OUT"
fi

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

echo "--- serial console log ---"
cat "$LOG"
echo "--- end log ---"

if [[ "$FOUND" -ne 1 ]]; then
    echo "FAIL: no real DISPLAY: PASS/FAIL marker appeared within ${TIMEOUT_S}s"
    exit 1
fi
if ! grep -q "DISPLAY: PASS" "$LOG"; then
    echo "FAIL: the real guest-side display probe itself reported FAIL (see log above)"
    exit 1
fi
if [[ ! -s "$PPM_OUT" ]]; then
    echo "FAIL: screendump produced no real output file"
    exit 1
fi

echo "--- verifying the real captured screenshot's real pixel content ---"
python3 "$SCRIPT_DIR/verify-screendump.py" "$PPM_OUT"
