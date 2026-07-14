#!/usr/bin/env bash
# docs/998-roadmap.md M6's real power-loss simulation: boots with a real, dedicated,
# pre-formatted ext4 data disk attached (a second virtio-blk drive, distinct from the boot disk),
# lets the guest's own hyperion-init start a real WAL write loop against it
# (crates/hyperion-init/src/linux/storage_probe.rs), then hard-kills qemu outright (SIGKILL, not
# a graceful shutdown -- a real power loss doesn't give the OS a chance to flush anything it
# hasn't already made durable) partway through. Reboots with the *same* data disk image and
# confirms hyperion-storage's own replay-on-open logic recovers a real, uncorrupted, in-range
# value -- never garbage, never silently partial -- proving the same crash-consistency guarantee
# every existing test already proves against a host tempfile, now against a real block device
# under a real abrupt kill.
#
# The data disk is attached with cache=none (O_DIRECT): this is load-bearing, not a performance
# tweak -- QEMU's default cache=writeback would let a guest's fsync "complete" once the write
# reaches the *host's* page cache, which a SIGKILL to the qemu process itself can still lose,
# making a "crash test" under that cache mode prove nothing about hyperion-storage's own
# discipline (it would just be measuring QEMU's write-back buffering instead). cache=none makes a
# completed guest fsync durable to the actual backing file the instant it returns, the same
# guarantee a real fsync gives on real hardware.
#
# Backgrounds qemu directly in this script's own top-level shell, never via `$(a_function)` --
# backgrounding a job *inside* a command-substitution subshell is a real, easy-to-hit bash
# footgun: the subshell exits (and can signal its own background children) the moment the
# function returns, independent of whether the job you meant to keep alive is actually done.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DISK_IMG="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output/images/disk.img}"
BOOT_WAIT_S="${STORAGE_TEST_BOOT_WAIT:-120}"
KILL_AFTER_WRITING_S="${STORAGE_TEST_KILL_AFTER:-5}"
REPLAY_WAIT_S="${STORAGE_TEST_TIMEOUT:-120}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

DATA_IMG="$(mktemp --suffix=.img)"
"$SCRIPT_DIR/create-data-disk.sh" "$DATA_IMG" 64M >/dev/null
VARS_COPY="$(mktemp --suffix=.fd)"
LOG1="$(mktemp)"
LOG2="$(mktemp)"
trap 'rm -f "$VARS_COPY" "$LOG1" "$LOG2" "$DATA_IMG"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$VARS_COPY"

QEMU_ARGS=(
    -M pc -m 2048 -smp 2 -display none
    -no-reboot
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE"
    -drive if=pflash,format=raw,file="$VARS_COPY"
    -drive file="$DISK_IMG",if=virtio,format=raw
    -drive file="$DATA_IMG",if=virtio,format=raw,cache=none
    -netdev user,id=net0
    -device virtio-net-pci,netdev=net0
)

# Polls `log` for `needle`, up to `timeout_s` real seconds, bailing out early (and printing why)
# if qemu itself has already exited. Echoes 1/0 on stdout so the caller can branch on it without
# depending on this function's own exit status surviving through more shell plumbing than needed.
wait_for() {
    local log="$1" needle="$2" timeout_s="$3" qemu_pid="$4"
    local waited=0
    while (( waited < timeout_s )); do
        if grep -q "$needle" "$log" 2>/dev/null; then
            echo 1
            return
        fi
        if ! kill -0 "$qemu_pid" 2>/dev/null; then
            echo 0
            return
        fi
        sleep 1
        (( waited += 1 ))
    done
    echo 0
}

echo "=== Phase 1: boot, wait for the guest's real WAL write loop to actually start, then hard-kill qemu ==="
qemu-system-x86_64 "${QEMU_ARGS[@]}" -serial file:"$LOG1" &
QEMU_PID=$!

STARTED=$(wait_for "$LOG1" "CRASH_TEST: fresh data partition, starting real WAL write loop" "$BOOT_WAIT_S" "$QEMU_PID")
if [[ "$STARTED" -ne 1 ]]; then
    echo "FAIL: the guest never started its real WAL write loop within ${BOOT_WAIT_S}s"
    kill -9 "$QEMU_PID" 2>/dev/null || true
    wait "$QEMU_PID" 2>/dev/null || true
    echo "--- phase 1 serial log ---"
    cat "$LOG1"
    echo "--- end phase 1 log ---"
    exit 1
fi

# The write loop is confirmed running; let it make some real, measurable progress before the
# kill, so phase 2 has something non-trivial to have recovered, not just record #1.
sleep "$KILL_AFTER_WRITING_S"

if ! kill -0 "$QEMU_PID" 2>/dev/null; then
    echo "FAIL: qemu exited on its own before the intended hard-kill"
    cat "$LOG1"
    exit 1
fi
kill -9 "$QEMU_PID"
wait "$QEMU_PID" 2>/dev/null || true

echo "--- phase 1 serial log ---"
cat "$LOG1"
echo "--- end phase 1 log ---"

if grep -q "CRASH_TEST: wrote all" "$LOG1"; then
    echo "FAIL: the write loop ran to completion before the kill -- this doesn't test a real \
mid-write interruption; lower STORAGE_TEST_KILL_AFTER or raise CRASH_TEST_WRITE_COUNT"
    exit 1
fi

echo "=== Phase 2: reboot with the SAME data disk, confirm a real, clean replay ==="
qemu-system-x86_64 "${QEMU_ARGS[@]}" -serial file:"$LOG2" &
QEMU_PID2=$!

FOUND=$(wait_for "$LOG2" "CRASH_TEST: replay result" "$REPLAY_WAIT_S" "$QEMU_PID2")
kill "$QEMU_PID2" 2>/dev/null || true
wait "$QEMU_PID2" 2>/dev/null || true

echo "--- phase 2 serial log ---"
cat "$LOG2"
echo "--- end phase 2 log ---"

if [[ "$FOUND" -ne 1 ]]; then
    echo "FAIL: the guest never reported a replay result on reboot within ${REPLAY_WAIT_S}s"
    exit 1
fi

if grep -q "CRASH_TEST: replay result: consistent" "$LOG2"; then
    echo "PASS: real WAL replay recovered a clean, uncorrupted value after a real hard power-loss kill"
    exit 0
else
    echo "FAIL: replay reported inconsistent/corrupt state after the kill"
    exit 1
fi
