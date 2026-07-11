#!/usr/bin/env bash
# Sourced (not executed) by other boot/scripts/*.sh to put a working
# qemu-system-x86_64 + OVMF on PATH and export OVMF_CODE/OVMF_VARS_TEMPLATE.
#
# Two environments are supported:
#   - A normal system with real package-manager access (e.g. CI, which runs
#     `apt-get install qemu-system-x86 ovmf` with real sudo): use those
#     system packages directly.
#   - This dev sandbox, which has no passwordless sudo: fall back to the
#     rootless extraction produced by setup-qemu-toolchain.sh.

QEMU_ENV_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QEMU_PREFIX="$(cd "$QEMU_ENV_SCRIPT_DIR/.." && pwd)/.tools/qemu-root"

_hyperion_find_system_ovmf() {
    for candidate in \
        /usr/share/OVMF/OVMF_CODE_4M.fd \
        /usr/share/OVMF/OVMF_CODE.fd \
        /usr/share/edk2/ovmf/OVMF_CODE.fd \
        /usr/share/edk2/x64/OVMF_CODE.fd
    do
        if [[ -f "$candidate" ]]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

if command -v qemu-system-x86_64 >/dev/null 2>&1 && SYSTEM_OVMF_CODE="$(_hyperion_find_system_ovmf)"; then
    export OVMF_CODE="$SYSTEM_OVMF_CODE"
    SYSTEM_OVMF_VARS="${SYSTEM_OVMF_CODE/CODE/VARS}"
    if [[ ! -f "$SYSTEM_OVMF_VARS" ]]; then
        echo "found $OVMF_CODE but no matching VARS file at $SYSTEM_OVMF_VARS" >&2
        return 1 2>/dev/null || exit 1
    fi
    export OVMF_VARS_TEMPLATE="$SYSTEM_OVMF_VARS"
elif [[ -f "$QEMU_PREFIX/.provisioned" ]]; then
    export PATH="$QEMU_PREFIX/usr/bin:$PATH"
    export LD_LIBRARY_PATH="$QEMU_PREFIX/usr/lib/x86_64-linux-gnu:$QEMU_PREFIX/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
    export OVMF_CODE="$QEMU_PREFIX/usr/share/OVMF/OVMF_CODE_4M.fd"
    export OVMF_VARS_TEMPLATE="$QEMU_PREFIX/usr/share/OVMF/OVMF_VARS_4M.fd"
else
    echo "no qemu-system-x86_64 + OVMF found: install them (apt-get install qemu-system-x86 ovmf)" >&2
    echo "or run boot/scripts/setup-qemu-toolchain.sh for a rootless local copy" >&2
    return 1 2>/dev/null || exit 1
fi
