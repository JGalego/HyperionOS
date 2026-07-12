#!/bin/sh
# Adds a getty on ttyAMA0 (the aarch64-virt machine's PL011 serial console -- the automated QEMU
# boot test's oracle, same role board/hyperion-x86_64/post-build.sh's ttyS0 getty plays there) in
# addition to whatever BR2_TARGET_GENERIC_GETTY_PORT already put on tty1.
set -u
set -e

BOARD_DIR="$(dirname "$0")"

if [ -e "${TARGET_DIR}/etc/inittab" ]; then
    grep -qE '^ttyAMA0::' "${TARGET_DIR}/etc/inittab" || \
        sed -i '/GENERIC_SERIAL/a\
ttyAMA0::respawn:/sbin/getty -L ttyAMA0 115200 vt100 # Hyperion boot-test console' \
        "${TARGET_DIR}/etc/inittab"
fi
