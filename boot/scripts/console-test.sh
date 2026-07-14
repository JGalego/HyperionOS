#!/usr/bin/env bash
# docs/998-roadmap.md M7 stage 1's exit criterion, proven for real: "a real utterance typed
# at the real booted console produces a real Intent Graph, a real Agent invocation, and real text
# output rendered to the real TTY." Boots the real image with ttyS0 backed by a real Unix domain
# socket (not boot-test.sh's `-serial file:...`, which only ever captures output -- this can also
# *send* a real typed line), drives it with console-drive.py, and asserts the real response
# contains what the real pipeline is expected to produce.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DISK_IMG="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output/images/disk.img}"
UTTERANCE="${2:-I need to launch my startup}"
EXPECT="${3:-market_research}"
TIMEOUT_S="${CONSOLE_TEST_TIMEOUT:-180}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

SOCK="$(mktemp -u --suffix=.sock)"
VARS_COPY="$(mktemp --suffix=.fd)"
trap 'rm -f "$VARS_COPY" "$SOCK"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$VARS_COPY"

echo "Booting $DISK_IMG (TCG), console on Unix socket $SOCK, utterance: \"$UTTERANCE\"..."

timeout "$TIMEOUT_S" qemu-system-x86_64 \
    -M pc \
    -m 2048 \
    -smp 2 \
    -display none \
    -no-reboot \
    -chardev socket,id=con0,path="$SOCK",server=on,wait=off \
    -serial chardev:con0 \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$VARS_COPY" \
    -drive file="$DISK_IMG",if=virtio,format=raw \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 &
QEMU_PID=$!

# The socket file only appears once qemu's chardev backend actually opens it -- poll rather than
# racing a fixed sleep against however long that (and the rest of qemu's own startup) takes.
for _ in $(seq 1 30); do
    [[ -S "$SOCK" ]] && break
    sleep 1
done

OUTPUT="$(python3 "$SCRIPT_DIR/console-drive.py" "$SOCK" "$UTTERANCE" "$TIMEOUT_S")"

kill "$QEMU_PID" 2>/dev/null || true
wait "$QEMU_PID" 2>/dev/null || true

echo "--- console session ---"
echo "$OUTPUT"
echo "--- end console session ---"

if echo "$OUTPUT" | grep -q "$EXPECT"; then
    echo "PASS: the real console's response to \"$UTTERANCE\" contains \"$EXPECT\""
    exit 0
else
    echo "FAIL: the real console's response to \"$UTTERANCE\" did not contain \"$EXPECT\""
    exit 1
fi
