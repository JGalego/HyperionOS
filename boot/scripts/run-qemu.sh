#!/usr/bin/env bash
# Interactive dev-loop boot of the Hyperion image in QEMU + OVMF (UEFI). No
# KVM acceleration is available in this dev environment (the sandbox user
# isn't in the `kvm` group), so this runs under TCG software emulation --
# slower than real hardware or a KVM host, but architecturally identical.
# Ctrl-a x to quit; Ctrl-a c for the QEMU monitor.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DISK_IMG="${1:-$BOOT_DIR/.tools/buildroot-2026.05/output/images/disk.img}"

# shellcheck source=./qemu-env.sh
source "$SCRIPT_DIR/qemu-env.sh"

# A fresh, writable copy of the firmware's variable store -- OVMF wants
# CODE (read-only firmware) and VARS (writable NVRAM) as separate files.
VARS_COPY="$(mktemp --suffix=.fd)"
trap 'rm -f "$VARS_COPY"' EXIT
cp "$OVMF_VARS_TEMPLATE" "$VARS_COPY"

exec qemu-system-x86_64 \
    -M pc \
    -m 2048 \
    -smp "$(nproc)" \
    -serial mon:stdio \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,file="$VARS_COPY" \
    -drive file="$DISK_IMG",if=virtio,format=raw \
    -netdev user,id=net0 \
    -device virtio-net-pci,netdev=net0 \
    "$@"
